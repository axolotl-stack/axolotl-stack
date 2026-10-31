//! Identity-bound same-world movement acknowledged by durable player storage.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;

use super::hooks::{TransferError, TransferQueue, owns_connection};
use super::transfer::{SourceBaseline, find_safe_destination, is_safe_destination, prepare_chunks};
use crate::entity::components::{PlayerSession, Position, Velocity};
use crate::server::game::player_events::ConnectionIdentity;
use crate::server::game::{self, GameServer, inventory_effects, item_use};

/// A completed same-world teleport after its canonical record was acknowledged.
#[derive(Event, Clone, Debug)]
pub struct PlayerRelocationCommitted {
    identity: ConnectionIdentity,
    pub player: Entity,
    pub request_id: String,
    pub from: DVec3,
    pub feet: DVec3,
}
impl PlayerRelocationCommitted {
    /// Read the original account and outbound channel independent of actor lifetime.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// A rejected move, or an uncertain write requiring explicit recovery.
#[derive(Event, Clone, Debug)]
pub struct PlayerRelocationFailed {
    identity: ConnectionIdentity,
    pub player: Entity,
    pub request_id: String,
    pub error: TransferError,
}
impl PlayerRelocationFailed {
    /// Read the exact original connection even if it left during destination loading.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// Post-boundary notifications run outside the registry borrow.
pub trait RelocationHandler: Send + Sync + 'static {
    /// Reconcile application state after actual acknowledged movement.
    fn on_committed(&self, _world: &mut World, _event: &PlayerRelocationCommitted) {}
    /// Release a reservation only when its failure is definite.
    fn on_failed(&self, _world: &mut World, _event: &PlayerRelocationFailed) {}
}

/// Ordered callbacks for native same-world relocation outcomes.
#[derive(Resource, Default)]
pub struct RelocationHooks {
    handlers: Vec<Arc<dyn RelocationHandler>>,
}
impl RelocationHooks {
    /// Register one ordered immutable lifecycle handler.
    pub fn register(&mut self, handler: impl RelocationHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

type Validator = Arc<dyn Fn(&World, DVec3) -> bool + Send + Sync>;
pub(super) struct RelocationRequest {
    pub(super) player: Entity,
    identity: ConnectionIdentity,
    feet: DVec3,
    radius: u32,
    id: String,
    remaining: u32,
    validator: Validator,
}
#[derive(Resource, Default)]
pub(super) struct RelocationQueue(pub Vec<RelocationRequest>);
#[derive(Resource, Default)]
pub(super) struct ActiveRelocations(pub std::collections::BTreeSet<Entity>);

/// Enqueue a same-world destination with no immediate state or success effects.
/// Normal teleport hooks and observers may veto or rewrite it. The predicate runs
/// only after final native revalidation, and may persist an application intent.
/// Exact destination player records are acknowledged before actual movement;
/// failures after a possible write terminally fence the original account.
pub fn request_relocation_checked(
    world: &mut World,
    player: Entity,
    feet: DVec3,
    radius: u32,
    request_id: String,
    final_validator: impl Fn(&World, DVec3) -> bool + Send + Sync + 'static,
) -> Result<(), TransferError> {
    if !feet.is_finite()
        || radius > 256
        || request_id.is_empty()
        || request_id.len() > 128
        || request_id.chars().any(char::is_control)
    {
        return Err(TransferError::InvalidRequest);
    }
    let identity =
        ConnectionIdentity::capture(world, player).ok_or(TransferError::SourceUnavailable)?;
    if !owns_connection(world, player, &identity) || super::is_loading(world, player) {
        return Err(TransferError::SourceUnavailable);
    }
    SourceBaseline::capture(world, player)?;
    if pending(world, player)
        || world
            .get_resource::<TransferQueue>()
            .is_some_and(|queue| queue.0.iter().any(|request| request.player == player))
    {
        return Err(TransferError::Pending);
    }
    world
        .get_resource_or_init::<RelocationQueue>()
        .0
        .push(RelocationRequest {
            player,
            identity,
            feet,
            radius,
            id: request_id,
            remaining: 200,
            validator: Arc::new(final_validator),
        });
    Ok(())
}

/// Same-world and dimension-changing requests share one pending actor contract.
pub(super) fn pending(world: &World, player: Entity) -> bool {
    world
        .get_resource::<ActiveRelocations>()
        .is_some_and(|active| active.0.contains(&player))
        || world
            .get_resource::<RelocationQueue>()
            .is_some_and(|queue| queue.0.iter().any(|request| request.player == player))
}

/// Snapshot listeners so callbacks can safely access their own registry.
fn handlers(world: &World) -> Vec<Arc<dyn RelocationHandler>> {
    world
        .get_resource::<RelocationHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}

/// Publish an immutable failure to handlers and observers exactly once.
fn failed(world: &mut World, request: RelocationRequest, error: TransferError) {
    let mut event = PlayerRelocationFailed {
        identity: request.identity,
        player: request.player,
        request_id: request.id,
        error,
    };
    for handler in handlers(world) {
        handler.on_failed(world, &event);
    }
    world.trigger_ref(&mut event);
    world.flush();
}

/// Retry terrain loading or outbound pressure without authorizing or consuming state.
fn retry(world: &mut World, mut request: RelocationRequest) {
    if request.remaining <= 1 {
        failed(world, request, TransferError::Timeout);
        return;
    }
    request.remaining -= 1;
    world
        .get_resource_or_init::<RelocationQueue>()
        .0
        .push(request);
}

/// Hold exclusive native world ownership through proposal, record acknowledgement
/// and movement, preventing intervening ordinary captures or inventory writes.
pub(super) async fn attempt(server: &mut GameServer, request: RelocationRequest) {
    if !owns_connection(server.ecs.world(), request.player, &request.identity) {
        failed(server.ecs.world_mut(), request, TransferError::StaleSource);
        return;
    }
    if !server
        .ecs
        .world()
        .contains_resource::<game::types::PlayerProviderResource>()
        && !server
            .ecs
            .world()
            .contains_resource::<game::types::PlayerDataStoreResource>()
    {
        failed(server.ecs.world_mut(), request, TransferError::Persistence);
        return;
    }
    match prepare_chunks(server.ecs.world_mut(), request.feet) {
        Ok(false) => {
            retry(server.ecs.world_mut(), request);
            return;
        }
        Err(error) => {
            failed(server.ecs.world_mut(), request, error);
            return;
        }
        Ok(true) => {}
    }
    let Some(destination) = find_safe_destination(server.ecs.world(), request.feet, request.radius)
    else {
        failed(
            server.ecs.world_mut(),
            request,
            TransferError::UnsafeDestination,
        );
        return;
    };
    let baseline = match SourceBaseline::capture(server.ecs.world(), request.player) {
        Ok(baseline) => baseline,
        Err(error) => {
            failed(server.ecs.world_mut(), request, error);
            return;
        }
    };
    let from = server
        .ecs
        .world()
        .get::<Position>(request.player)
        .unwrap()
        .0
        - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    let Some(permit) = crate::entity::projectile::hooks::teleport_with_request(
        server.ecs.world_mut(),
        request.player,
        None,
        item_use::TeleportCause::Command,
        destination,
        Some(request.id.clone()),
    ) else {
        failed(server.ecs.world_mut(), request, TransferError::Rejected);
        return;
    };
    if !owns_connection(server.ecs.world(), request.player, &request.identity)
        || !baseline.matches(server.ecs.world(), request.player)
    {
        failed(server.ecs.world_mut(), request, TransferError::StaleSource);
        return;
    }
    if !is_safe_destination(server.ecs.world(), permit.destination) {
        failed(
            server.ecs.world_mut(),
            request,
            TransferError::UnsafeDestination,
        );
        return;
    }
    let outbound = server
        .ecs
        .world()
        .get::<PlayerSession>(request.player)
        .unwrap()
        .outbound_tx
        .clone();
    let reservation = match outbound.try_reserve() {
        Ok(reservation) => reservation,
        Err(_) => {
            retry(server.ecs.world_mut(), request);
            return;
        }
    };
    let recovery =
        match inventory_effects::RecoverySource::capture(server.ecs.world(), request.player) {
            Ok(recovery) => recovery,
            Err(_) => {
                failed(
                    server.ecs.world_mut(),
                    request,
                    TransferError::SourceUnavailable,
                );
                return;
            }
        };
    let Some((uuid, mut record)) = game::player_record(server.ecs.world(), request.player) else {
        failed(
            server.ecs.world_mut(),
            request,
            TransferError::SourceUnavailable,
        );
        return;
    };
    record.position = (permit.destination + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT).to_array();
    if !(request.validator)(server.ecs.world(), permit.destination) {
        failed(server.ecs.world_mut(), request, TransferError::Rejected);
        return;
    }
    if !server.save_player_record(uuid, &record).await {
        let _ = inventory_effects::fence_for_recovery(server.ecs.world_mut(), &recovery);
        failed(
            server.ecs.world_mut(),
            request,
            TransferError::RecoveryRequired,
        );
        return;
    }
    let destination = permit.destination;
    if let Some(mut velocity) = server.ecs.world_mut().get_mut::<Velocity>(request.player) {
        velocity.0 = DVec3::ZERO;
    }
    if let Some(mut fall) =
        server
            .ecs
            .world_mut()
            .get_mut::<crate::entity::player_life::environment::PlayerFall>(request.player)
    {
        *fall = Default::default();
    }
    if item_use::commit_teleport(
        server.ecs.world_mut(),
        request.player,
        item_use::TeleportCause::Command,
        permit,
        Some(reservation),
    )
    .is_none()
    {
        let _ = inventory_effects::fence_for_recovery(server.ecs.world_mut(), &recovery);
        failed(
            server.ecs.world_mut(),
            request,
            TransferError::RecoveryRequired,
        );
        return;
    }
    let mut event = PlayerRelocationCommitted {
        identity: request.identity,
        player: request.player,
        request_id: request.id,
        from,
        feet: destination,
    };
    for handler in handlers(server.ecs.world()) {
        handler.on_committed(server.ecs.world_mut(), &event);
    }
    server.ecs.world_mut().trigger_ref(&mut event);
    server.ecs.world_mut().flush();
}

#[cfg(test)]
mod tests;
