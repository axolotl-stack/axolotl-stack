//! Synchronous component session policy at native publication and teardown boundaries.

use super::{
    PluginManager, make_player_resource, make_world_tx, players, prepare_invocation,
    wit_types as wit, with_world,
};
use crate::ecs::events::EventKind;
use crate::entity::components::{PlayerSession, RuntimeEntityId};
use crate::server::game::player_events::{
    self, ConnectionIdentity, PlayerEventHandler, PlayerEventHooks, PlayerJoined, PlayerQuit,
};
use bevy_ecs::prelude::*;
use std::sync::{Arc, Mutex};

#[path = "sessions/holds.rs"]
pub(crate) mod holds;
use holds::{AdmissionBook, AdmissionGuard, AdmissionId, Decision, HoldId, Status};

/// One proposal's authority exists only inside its original admission callback.
#[derive(Clone)]
pub(super) struct ActiveAdmission {
    pub(super) book: AdmissionBook,
    pub(super) id: AdmissionId,
    pub(super) player: Entity,
}
/// Owned resources retain bookkeeping, never a borrowed World or scoped player.
#[derive(Clone)]
pub(super) struct OwnedHold {
    pub(super) book: AdmissionBook,
    pub(super) id: HoldId,
    pub(super) player: Entity,
    pub(super) owner: String,
    pub(super) staged: bool,
}
/// A successful host import remains provisional until its enclosing callback succeeds.
pub(super) struct StagedResolution {
    pub(super) hold: OwnedHold,
    pub(super) decision: Decision,
}

#[derive(Resource, Clone)]
pub(super) struct SessionBridge {
    manager: Arc<Mutex<PluginManager>>,
}
/// Session admission survives native realm migration but never a replacement connection.
#[derive(Component, Clone)]
struct AdmittedConnection {
    identity: ConnectionIdentity,
    runtime: i64,
}

/// Register internal observations without adding another native application surface.
pub(super) fn install(manager: Arc<Mutex<PluginManager>>, world: &mut World) {
    world.init_resource::<PlayerEventHooks>();
    world.init_resource::<AdmissionBook>();
    let bridge = SessionBridge { manager };
    world.insert_resource(bridge.clone());
    world.resource_mut::<PlayerEventHooks>().register(bridge);
}
/// Native transfers retain the already-admitted connection's observation marker.
pub(crate) fn register_transfer_components(
    components: &mut crate::server::worlds::TransferComponents,
) {
    components.preserve::<AdmittedConnection>();
}

impl PlayerEventHandler for SessionBridge {
    fn on_joined(&self, world: &mut World, event: &PlayerJoined) {
        let book = world.resource::<AdmissionBook>().clone();
        let started = match self.manager.try_lock() {
            Ok(mut manager) => {
                if manager.subscribers(EventKind::PlayerJoin).is_empty() {
                    return;
                }
                let Ok(id) = book.begin(world, event.player()) else {
                    drop(manager);
                    reject(
                        world,
                        event.player(),
                        event.identity(),
                        "Unable to complete connection setup.",
                    );
                    return;
                };
                manager.admission(
                    world,
                    event,
                    ActiveAdmission {
                        book: book.clone(),
                        id,
                        player: event.player(),
                    },
                );
                Some(id)
            }
            Err(error) => {
                tracing::error!(%error,"Component manager unavailable during admission");
                drop(error);
                reject(
                    world,
                    event.player(),
                    event.identity(),
                    "Connection setup is unavailable.",
                );
                None
            }
        };
        if started.is_some() {
            reconcile(world);
        }
    }
    fn on_quit(&self, world: &mut World, event: &mut PlayerQuit) {
        if !event.identity().matches(world, event.player()) {
            return;
        }
        if let Some(guard) = world.get::<AdmissionGuard>(event.player()) {
            // Invalidate holds but keep the guard until teardown: an unadmitted
            // departure must not make its incomplete state eligible for autosave.
            guard.book.remove(guard.id);
        }
        match self.manager.try_lock() {
            Ok(mut manager) => manager.quitting(world, event),
            Err(error) => {
                tracing::error!(%error,"Component manager unavailable during departure");
                super::readiness::fence_world(world);
            }
        }
    }
}

impl PluginManager {
    /// Aggregate component admission decisions before any initial state can be visible.
    fn admission(&mut self, world: &mut World, event: &PlayerJoined, active: ActiveAdmission) {
        if !self.begin_tick(world) {
            active
                .book
                .reject(active.id, "Connection setup is unavailable.");
            let _ = active.book.evaluated(active.id);
            return;
        }
        for owner in self.subscribers(EventKind::PlayerJoin) {
            if !event.identity().matches(world, event.player()) {
                break;
            }
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            plugin.store.data_mut().pending_actions.clear();
            let result = with_world(&mut plugin.store, world, |store| -> anyhow::Result<_> {
                prepare_invocation(store);
                store.data_mut().active_admission = Some(active.clone());
                let tx = make_world_tx(store)?;
                let player = make_player_resource(store, event.player())?;
                plugin.bindings.call_on_player_admission(store, tx, player)
            });
            let decision = result.and_then(|decision| {
                let decision = match decision {
                    wit::AdmissionDecision::Allow => Decision::Allow,
                    wit::AdmissionDecision::Reject(reason) => Decision::Reject(reason),
                    wit::AdmissionDecision::Deferred => Decision::Deferred,
                };
                active
                    .book
                    .decide(active.id, &owner.0, decision)
                    .map_err(|error| anyhow::anyhow!("invalid admission decision: {error:?}"))
            });
            super::finish_invocation(plugin.store.data_mut(), world, decision.is_ok());
            plugin.store.data_mut().pending_actions.clear();
            if let Err(error) = decision {
                active
                    .book
                    .reject(active.id, "Unable to complete connection setup.");
                self.callback_failed(world, &owner, &super::wasm_failure_reason(&error));
                break;
            }
            if matches!(active.book.status(world, active.id), Status::Rejected(_)) {
                break;
            }
        }
        let _ = active.book.evaluated(active.id);
    }

    /// Supply owned pre-teardown facts with a fresh transaction, never a queued live Player.
    fn quitting(&mut self, world: &mut World, event: &mut PlayerQuit) {
        if !self.begin_tick(world) {
            return;
        }
        let Ok(identity) = players::owned_identity(world, event.player()) else {
            return;
        };
        let observation = players::owned_snapshot(world, event.player());
        let record = wit::QuitEvent {
            identity,
            dimension: world
                .resource::<crate::world::ecs::ChunkManager>()
                .dimension(),
            observation,
            reason: departure_reason(
                world.get::<crate::network::departure::QuitCause>(event.player()),
            ),
            previously_admitted: admitted(world, event.player()),
        };
        let mut retain = false;
        for owner in self.subscribers(EventKind::PlayerQuit) {
            if !event.identity().matches(world, event.player()) {
                return;
            }
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            plugin.store.data_mut().pending_actions.clear();
            let result = with_world(&mut plugin.store, world, |store| -> anyhow::Result<_> {
                prepare_invocation(store);
                let tx = make_world_tx(store)?;
                plugin.bindings.call_on_player_quit(store, tx, &record)
            });
            super::finish_invocation(plugin.store.data_mut(), world, result.is_ok());
            plugin.store.data_mut().pending_actions.clear();
            match result {
                Ok(wit::QuitDisposition::RetainEntity) => retain = true,
                Ok(wit::QuitDisposition::Remove) => (),
                Err(error) => {
                    self.callback_failed(world, &owner, &super::wasm_failure_reason(&error));
                    if self.readiness.check().is_err() {
                        return;
                    }
                }
            }
        }
        if retain && event.identity().matches(world, event.player()) {
            event.retain_entity();
        }
    }

    /// Observe committed publication exactly once per original connection.
    fn admitted_notification(
        &mut self,
        world: &mut World,
        player: Entity,
        observation: wit::PlayerSnapshot,
    ) {
        if !self.begin_tick(world) {
            return;
        }
        let record = wit::AdmittedEvent { observation };
        for owner in self.subscribers(EventKind::PlayerAdmitted) {
            let plugin = self.plugins.get_mut(&owner).expect("subscribed component");
            let result = with_world(&mut plugin.store, world, |store| {
                prepare_invocation(store);
                plugin.bindings.call_on_player_admitted(store, &record)
            });
            super::finish_invocation(plugin.store.data_mut(), world, result.is_ok());
            if let Err(error) = result {
                self.callback_failed(world, &owner, &super::wasm_failure_reason(&error));
                if self.readiness.check().is_err() {
                    break;
                }
            }
        }
        let _ = player;
    }
}

/// Preserve wire reason facts as owned read-only metadata, independent of host enum layout.
fn departure_reason(cause: Option<&crate::network::departure::QuitCause>) -> wit::DisconnectReason {
    use crate::network::departure::QuitCause;
    let detail = |reason: Option<jolyne::valentine::EnumsConnectionDisconnectFailReason>,
                  message: Option<String>| wit::DisconnectDetail {
        protocol_code: reason.and_then(protocol_code),
        message,
    };
    match cause {
        Some(QuitCause::PeerDisconnect { reason, message }) => {
            wit::DisconnectReason::PeerRequest(detail(Some(*reason), message.clone()))
        }
        Some(QuitCause::ServerDisconnect { reason, message }) => {
            wit::DisconnectReason::ServerRequest(detail(*reason, message.clone()))
        }
        Some(QuitCause::PeerClosed) => wit::DisconnectReason::PeerClosed,
        Some(QuitCause::ServerShutdown) => wit::DisconnectReason::ServerShutdown,
        Some(QuitCause::QueueOverflow) => wit::DisconnectReason::Overloaded,
        Some(QuitCause::TransportFailure { reason, message }) => {
            wit::DisconnectReason::TransportFailure(detail(*reason, Some(message.clone())))
        }
        None | Some(QuitCause::Unknown) => wit::DisconnectReason::Unknown,
    }
}
/// Reuse the selected protocol codec so known and unknown signed codes remain exact.
fn protocol_code(reason: jolyne::valentine::EnumsConnectionDisconnectFailReason) -> Option<i32> {
    use jolyne::valentine::bedrock::codec::{BedrockCodec, ZigZag32};
    let mut bytes = Vec::new();
    reason.encode(&mut bytes).ok()?;
    ZigZag32::decode(&mut bytes.as_slice(), ())
        .ok()
        .map(|code| code.0)
}
/// Read only a marker bound to this same original connection and runtime actor.
fn admitted(world: &World, player: Entity) -> bool {
    world
        .get::<AdmittedConnection>(player)
        .is_some_and(|marker| {
            marker.identity.matches(world, player)
                && world.get::<RuntimeEntityId>(player).map(|id| id.0) == Some(marker.runtime)
        })
}
/// Native publication consults the hold map independently of the legacy ready bit.
pub(crate) fn blocks_publication(world: &World, player: Entity) -> bool {
    holds::blocks_publication(world, player)
}

/// Release accepted complete plans or reject failed sources outside every manager borrow.
pub(crate) fn reconcile(world: &mut World) {
    if let Some(book) = world.get_resource::<AdmissionBook>().cloned() {
        book.prune(world);
    }
    let guarded: Vec<_> = world
        .query::<(Entity, &AdmissionGuard)>()
        .iter(world)
        .map(|(player, guard)| (player, guard.id, guard.book.clone()))
        .collect();
    for (player, id, book) in guarded {
        match book.status(world, id) {
            Status::Ready => {
                player_events::release_initial_state(world, player);
            }
            Status::Rejected(reason) => {
                if let Some(identity) = ConnectionIdentity::capture(world, player) {
                    reject(world, player, &identity, &reason);
                }
            }
            Status::Stale => {
                book.remove(id);
            }
            Status::Waiting => (),
        }
    }
}

/// Reject only the original source; teardown still uses its ordinary native observers.
fn reject(world: &mut World, player: Entity, identity: &ConnectionIdentity, reason: &str) {
    use jolyne::valentine::{
        DisconnectPacket, DisconnectPacketMessages, DisconnectPacketMessagesUnion,
        EnumsConnectionDisconnectFailReason,
    };
    if !identity.matches(world, player) {
        return;
    }
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(
            DisconnectPacket {
                reason: EnumsConnectionDisconnectFailReason::Disconnected,
                messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                    DisconnectPacketMessages {
                        message: reason.into(),
                        filtered_message: reason.into(),
                    },
                ),
            }
            .into(),
        );
    }
    world
        .entity_mut(player)
        .insert(crate::network::departure::QuitCause::ServerDisconnect {
            reason: Some(EnumsConnectionDisconnectFailReason::Disconnected),
            message: Some(reason.into()),
        });
    player_events::disconnect(world, identity.session_id());
}

/// Emit owned committed facts after successful native initial enqueue, never on attempts.
pub(crate) fn published(world: &mut World, player: Entity) {
    if world
        .get::<player_events::InitialStatePublished>(player)
        .is_none()
        || admitted(world, player)
    {
        return;
    }
    let Some(bridge) = world.get_resource::<SessionBridge>().cloned() else {
        return;
    };
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return;
    };
    let Some(runtime) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
        return;
    };
    let Ok(observation) = players::owned_snapshot(world, player) else {
        return;
    };
    if let Some(guard) = world.entity_mut(player).take::<AdmissionGuard>() {
        guard.book.remove(guard.id);
    }
    world
        .entity_mut(player)
        .insert(AdmittedConnection { identity, runtime });
    match bridge.manager.try_lock() {
        Ok(mut manager) => manager.admitted_notification(world, player, observation),
        Err(error) => {
            tracing::error!(%error,"Component manager unavailable after initial publication");
            super::readiness::fence_world(world);
        }
    }
}
