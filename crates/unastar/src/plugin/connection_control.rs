//! Bounded component-owned termination requests, published outside guest and manager borrows.

use super::{
    HostContext,
    players::{PlayerLineage, WorldReference},
    wit_types as wit,
};
use crate::entity::components::PlayerSession;
use crate::server::game::{
    SessionEntityMap,
    player_events::{self, ConnectionIdentity},
};
use bevy_ecs::prelude::*;
use parking_lot::Mutex;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicU32, Ordering},
};
use std::time::{Duration, Instant};

/// Hard native bounds also apply when a component requests larger soft limits.
pub(super) const MAX_PENDING: usize = 4096;
pub(super) const MAX_REASON: usize = 4096;
const RETRY_WINDOW: Duration = Duration::from_secs(10);

/// A ticket reports host staging/delivery, never a client acknowledgement.
pub(super) struct RequestState(Mutex<wit::ConnectionCloseStatus>);
impl RequestState {
    /// Construct state before publishing its owned guest resource.
    pub(super) fn new() -> Self {
        Self(Mutex::new(wit::ConnectionCloseStatus::Staged))
    }
    /// Observe owned state even after the original actor disappears.
    pub(super) fn status(&self) -> wit::ConnectionCloseStatus {
        *self.0.lock()
    }
    /// Set a terminal or pending result without requiring any guest re-entry.
    fn set(&self, status: wit::ConnectionCloseStatus) {
        *self.0.lock() = status;
    }
    /// End an unfulfilled request without overwriting an already queued result.
    pub(super) fn fail(&self, error: wit::ConnectionControlError) {
        let mut status = self.0.lock();
        if matches!(
            *status,
            wit::ConnectionCloseStatus::Staged | wit::ConnectionCloseStatus::Pending
        ) {
            *status = wit::ConnectionCloseStatus::Failed(error);
        }
    }
}
/// A quota reservation survives ticket drop and releases only when native work ends.
pub(super) struct Reservation(Arc<AtomicU32>);
impl Reservation {
    /// Reserve one pending request atomically, independent of guest resource ownership.
    pub(super) fn acquire(
        counter: &Arc<AtomicU32>,
        maximum: u32,
    ) -> Result<Self, wit::AccessError> {
        counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                (current < maximum.min(MAX_PENDING as u32)).then(|| current + 1)
            })
            .map_err(|_| wit::AccessError::LimitExceeded)?;
        Ok(Self(counter.clone()))
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Immutable authority copied from one live world transaction.
pub(super) struct Source {
    pub(super) reference: PlayerLineage,
    pub(super) world: WorldReference,
    pub(super) player: Entity,
    pub(super) connection: ConnectionIdentity,
}
/// Every retry retains the original actor generation, realm and exact authenticated transport.
pub(super) struct Request {
    pub(super) state: Arc<RequestState>,
    pub(super) source: Source,
    pub(super) reason: wit::ConnectionCloseReason,
    pub(super) owner: Weak<()>,
    _reservation: Reservation,
    deadline: Instant,
}
impl Request {
    /// Set a finite native retry interval before the callback relinquishes its world scope.
    pub(super) fn new_deadline() -> Instant {
        Instant::now() + RETRY_WINDOW
    }
    /// Assemble owned work after all validation and quota reservation succeeds.
    pub(super) fn new(
        state: Arc<RequestState>,
        source: Source,
        reason: wit::ConnectionCloseReason,
        owner: Weak<()>,
        reservation: Reservation,
    ) -> Self {
        Self {
            state,
            source,
            reason,
            owner,
            _reservation: reservation,
            deadline: Self::new_deadline(),
        }
    }
    /// No stale or migrated actor inherits authority merely by reusing a session number.
    fn current(&self, world: &World) -> Result<(), wit::ConnectionControlError> {
        use wit::{AccessError as Access, ConnectionControlError as Error};
        if self.owner.upgrade().is_none() {
            return Err(Error::Access(Access::Cancelled));
        }
        if !self.source.world.matches(world) {
            return Err(Error::Access(Access::WrongWorld));
        }
        if !self.source.reference.matches(world, self.source.player) {
            return Err(Error::Access(Access::StalePlayer));
        }
        let session = world
            .get::<PlayerSession>(self.source.player)
            .ok_or(Error::NotConnected)?;
        if !self.source.connection.matches(world, self.source.player)
            || world
                .get_resource::<SessionEntityMap>()
                .and_then(|map| map.get(session.session_id))
                != Some(self.source.player)
        {
            return Err(Error::Access(Access::StalePlayer));
        }
        if player_events::initial_state_pending(world, self.source.player) {
            return Err(Error::AdmissionPending);
        }
        Ok(())
    }
    /// Hand the terminal packet to the authoritative pump, retaining backpressure without dropping it.
    fn publish(&self, world: &World, now: Instant) -> Result<bool, wit::ConnectionControlError> {
        self.current(world)?;
        if now >= self.deadline {
            return Err(wit::ConnectionControlError::TimedOut);
        }
        let session = world
            .get::<PlayerSession>(self.source.player)
            .ok_or(wit::ConnectionControlError::NotConnected)?;
        let permit = match session.outbound_tx.try_reserve() {
            Ok(permit) => permit,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => return Ok(false),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                return Err(wit::ConnectionControlError::NotConnected);
            }
        };
        use jolyne::valentine::{
            DisconnectPacket, DisconnectPacketMessages, DisconnectPacketMessagesUnion,
            EnumsConnectionDisconnectFailReason,
        };
        let message = self.reason.message.as_str();
        permit.send(
            DisconnectPacket {
                reason: match self.reason.kind {
                    wit::ConnectionCloseKind::Requested => {
                        EnumsConnectionDisconnectFailReason::Disconnected
                    }
                    wit::ConnectionCloseKind::Kicked => EnumsConnectionDisconnectFailReason::Kicked,
                },
                messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                    DisconnectPacketMessages {
                        message: message.into(),
                        filtered_message: message.into(),
                    },
                ),
            }
            .into(),
        );
        self.state.set(wit::ConnectionCloseStatus::Queued);
        Ok(true)
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        self.state.fail(wit::ConnectionControlError::Access(
            wit::AccessError::Cancelled,
        ));
    }
}

/// World-local work never follows a reconnect or grants cross-world mutation authority.
#[derive(Resource, Default)]
pub(super) struct Queue(pub(super) Vec<Request>);
impl Drop for Queue {
    fn drop(&mut self) {
        for request in &self.0 {
            request.state.fail(wit::ConnectionControlError::Access(
                wit::AccessError::WorldClosed,
            ));
        }
    }
}

/// Reserve against both staged and committed requests; duplicate termination is explicit.
pub(super) fn capacity(
    world: &World,
    player: Entity,
    staged: &[Request],
) -> Result<(), wit::ConnectionControlError> {
    let pending = world.get_resource::<Queue>();
    let queued = pending.map_or(0, |queue| queue.0.len());
    if queued + staged.len() >= MAX_PENDING {
        return Err(wit::ConnectionControlError::Access(
            wit::AccessError::LimitExceeded,
        ));
    }
    if staged.iter().any(|request| request.source.player == player)
        || pending.is_some_and(|queue| {
            queue
                .0
                .iter()
                .any(|request| request.source.player == player && request.owner.upgrade().is_some())
        })
    {
        return Err(wit::ConnectionControlError::AlreadyPending);
    }
    Ok(())
}
/// Commit staging only after a successful callback; explicit default-action cancellation still commits.
pub(super) fn finish(context: &mut HostContext, world: &mut World, succeeded: bool) {
    let requests = std::mem::take(&mut context.pending_connection_closes);
    if !succeeded {
        for request in requests {
            request.state.fail(wit::ConnectionControlError::Access(
                wit::AccessError::CallbackFailed,
            ));
        }
        return;
    }
    if requests.is_empty() {
        return;
    }
    world.init_resource::<Queue>();
    for request in requests {
        if let Err(error) = request.current(world) {
            request.state.fail(error);
            continue;
        }
        if let Err(error) = capacity(world, request.source.player, &[]) {
            request.state.fail(error);
            continue;
        }
        request.state.set(wit::ConnectionCloseStatus::Pending);
        world.resource_mut::<Queue>().0.push(request);
    }
}
/// Run with no manager lock and no guest export active. The network pump performs actual close.
pub(crate) fn publish(world: &mut World) {
    publish_at(world, Instant::now());
}
/// An explicit native clock makes backpressure and deadline tests deterministic.
pub(super) fn publish_at(world: &mut World, now: Instant) {
    let Some(mut queue) = world.get_resource_mut::<Queue>() else {
        return;
    };
    let requests = std::mem::take(&mut queue.0);
    let mut retry = Vec::new();
    for request in requests {
        match request.publish(world, now) {
            Ok(true) => (),
            Ok(false) => retry.push(request),
            Err(error) => request.state.fail(error),
        }
    }
    world.resource_mut::<Queue>().0 = retry;
}
