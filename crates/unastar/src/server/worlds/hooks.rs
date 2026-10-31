//! Source-owned callbacks and identity-bound requests for native world transfers.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use thiserror::Error;

use crate::server::game::player_events::ConnectionIdentity;

/// A loaded, collision-checked destination proposal before player state moves.
/// Native handlers may cancel or adjust feet coordinates in the selected world.
/// The engine checks geometry and the caller's final predicate after all handlers.
#[derive(Event, Clone, Debug)]
pub struct PlayerWorldTransfer {
    pub(super) player: Entity,
    pub(super) identity: ConnectionIdentity,
    pub(super) request_id: String,
    pub(super) from_dimension: i32,
    pub(super) to_dimension: i32,
    pub(super) from: DVec3,
    pub feet: DVec3,
    pub(super) cancelled: bool,
}
impl PlayerWorldTransfer {
    /// Read the actor in the source ECS world.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the original authenticated account and transport identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
    /// Read the embedding application's opaque request identifier.
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    /// Read the actual source dimension.
    pub fn from_dimension(&self) -> i32 {
        self.from_dimension
    }
    /// Read the immutable selected destination dimension.
    pub fn to_dimension(&self) -> i32 {
        self.to_dimension
    }
    /// Read actual source feet captured before callbacks.
    pub fn from(&self) -> DVec3 {
        self.from
    }
    /// Veto this operation without moving entities or changing persisted position.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether an earlier native handler or observer vetoed the operation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// An actual completed migration. Actor IDs are scoped to their respective ECS
/// worlds; stable account, transport and runtime identities are preserved.
#[derive(Event, Clone, Debug)]
pub struct PlayerWorldTransferred {
    pub(super) identity: ConnectionIdentity,
    pub request_id: String,
    pub previous_player: Entity,
    pub player: Entity,
    pub from_dimension: i32,
    pub to_dimension: i32,
    pub from: DVec3,
    pub feet: DVec3,
}
impl PlayerWorldTransferred {
    /// Read the immutable original connection after its owned component moves.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// A rejected or failed request that did not migrate the original player.
#[derive(Event, Clone, Debug)]
pub struct WorldTransferFailed {
    pub(super) identity: ConnectionIdentity,
    pub player: Entity,
    pub request_id: String,
    pub destination_dimension: i32,
    pub error: TransferError,
}
impl WorldTransferFailed {
    /// Read the original identity even if its source actor left during loading.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
}

/// Source-world handlers run without a registry/component borrow and may read
/// both worlds. The committed callback runs while destination publication remains
/// deferred, so projections containing world-local identities can be rebuilt.
pub trait WorldTransferHandler: Send + Sync + 'static {
    /// Authorize or redirect a loaded destination before persistence and movement.
    fn on_transfer(
        &self,
        _source: &mut World,
        _destination: &mut World,
        _event: &mut PlayerWorldTransfer,
    ) {
    }
    /// Reconcile application projections after ownership moved and before visibility.
    fn on_committed(
        &self,
        _source: &mut World,
        _destination: &mut World,
        _event: &PlayerWorldTransferred,
    ) {
    }
    /// Reconcile an internal reservation for an uncommitted request.
    fn on_failed(&self, _source: &mut World, _event: &WorldTransferFailed) {}
}

/// Ordered source-world policy and lifecycle callbacks.
#[derive(Resource, Clone, Default)]
pub struct WorldTransferHooks {
    handlers: Vec<Arc<dyn WorldTransferHandler>>,
}
impl WorldTransferHooks {
    /// Register one handler for subsequent source-world dispatches.
    pub fn register(&mut self, handler: impl WorldTransferHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// A native transfer failure, before a replacement actor becomes visible.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TransferError {
    #[error("invalid or duplicate dimension")]
    InvalidDimension,
    #[error("destination dimension is unavailable")]
    UnknownDimension,
    #[error("world registries are incompatible with the existing connection")]
    IncompatibleRegistries,
    #[error("source player state is unavailable")]
    SourceUnavailable,
    #[error("source identity or state changed")]
    StaleSource,
    #[error("invalid or unsafe destination")]
    UnsafeDestination,
    #[error("world transfer was rejected")]
    Rejected,
    #[error("another transfer is already pending")]
    Pending,
    #[error("invalid transfer request identifier or bounds")]
    InvalidRequest,
    #[error("unsupported player component migration: {0:?}")]
    UnsupportedComponents(Vec<String>),
    #[error("outbound transport is temporarily full")]
    Backpressure,
    #[error("destination loading timed out")]
    Timeout,
    #[error("player persistence failed")]
    Persistence,
    #[error("player storage may have changed; explicit recovery is required")]
    RecoveryRequired,
}

pub(super) type Validator = Arc<dyn Fn(&World, &World, &PlayerWorldTransfer) -> bool + Send + Sync>;

/// One identity-bound native request; callbacks and tokens stay process-local.
pub(super) struct TransferRequest {
    pub player: Entity,
    pub identity: ConnectionIdentity,
    pub destination_dimension: i32,
    pub feet: DVec3,
    pub safe_y_radius: u32,
    pub request_id: String,
    pub validator: Validator,
    pub remaining_ticks: u32,
}
#[derive(Resource, Default)]
pub(super) struct TransferQueue(pub Vec<TransferRequest>);

/// Queue native loading and safe-Y preparation. No player effects occur here.
/// The predicate runs after proposal callbacks and final host revalidation; use
/// the committed notification to publish application success. The source registry
/// owns outgoing policy. `safe_y_radius` is bounded to 256 blocks and requests
/// time out after 200 router ticks. One request may be pending per source actor.
pub fn request_transfer_checked(
    world: &mut World,
    player: Entity,
    destination_dimension: i32,
    feet: DVec3,
    safe_y_radius: u32,
    request_id: String,
    validator: impl Fn(&World, &World, &PlayerWorldTransfer) -> bool + Send + Sync + 'static,
) -> Result<(), TransferError> {
    if !(0..=2).contains(&destination_dimension)
        || !feet.is_finite()
        || safe_y_radius > 256
        || request_id.is_empty()
        || request_id.len() > 128
        || request_id.chars().any(char::is_control)
    {
        return Err(TransferError::InvalidRequest);
    }
    let identity =
        ConnectionIdentity::capture(world, player).ok_or(TransferError::SourceUnavailable)?;
    if !owns_connection(world, player, &identity)
        || super::is_loading(world, player)
        || crate::server::game::player_events::initial_state_pending(world, player)
        || crate::server::game::inventory_effects::is_quarantined(world, player)
        || crate::server::game::player_state::capture(world, player).is_err()
    {
        return Err(TransferError::SourceUnavailable);
    }
    if super::relocate::pending(world, player) {
        return Err(TransferError::Pending);
    }
    let mut queue = world.get_resource_or_init::<TransferQueue>();
    if queue.0.iter().any(|pending| pending.player == player) {
        return Err(TransferError::Pending);
    }
    queue.0.push(TransferRequest {
        player,
        identity,
        destination_dimension,
        feet,
        safe_y_radius,
        request_id,
        validator: Arc::new(validator),
        remaining_ticks: 200,
    });
    Ok(())
}

/// Snapshot source callbacks without retaining their resource borrow.
fn handlers(world: &World) -> Vec<Arc<dyn WorldTransferHandler>> {
    world
        .get_resource::<WorldTransferHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}
/// Dispatch synchronous authorization followed by source-world observers.
pub(super) fn proposed(
    source: &mut World,
    target: &mut World,
    event: &mut PlayerWorldTransfer,
) -> bool {
    for handler in handlers(source) {
        handler.on_transfer(source, target, event);
    }
    source.trigger_ref(event);
    !event.cancelled
}
/// Notify native callbacks and destination observers before deferred publication.
pub(super) fn committed(source: &mut World, target: &mut World, mut event: PlayerWorldTransferred) {
    for handler in handlers(source) {
        handler.on_committed(source, target, &event);
    }
    target.trigger_ref(&mut event);
}
/// Notify only uncommitted attempts; the engine has retained source ownership.
pub(super) fn failed(world: &mut World, request: TransferRequest, error: TransferError) {
    let mut event = WorldTransferFailed {
        identity: request.identity,
        player: request.player,
        request_id: request.request_id,
        destination_dimension: request.destination_dimension,
        error,
    };
    for handler in handlers(world) {
        handler.on_failed(world, &event);
    }
    world.trigger_ref(&mut event);
}

/// Require native session-map ownership in addition to the immutable channel.
pub(super) fn owns_connection(
    world: &World,
    player: Entity,
    identity: &ConnectionIdentity,
) -> bool {
    world
        .get_resource::<crate::server::game::SessionEntityMap>()
        .is_some_and(|map| map.get(identity.session_id()) == Some(player))
        && identity.matches(world, player)
}
