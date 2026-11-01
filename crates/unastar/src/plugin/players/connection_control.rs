//! Scoped connection imports reuse live-resource guards without requiring mutable gameplay state.

use super::super::connection_control::{
    self as control, Request, RequestState, Reservation, Source,
};
use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{
    SessionEntityMap,
    player_events::{self, ConnectionIdentity},
};

impl HostContext {
    /// Validate reason, exact transport and both quotas before allocating or staging native work.
    fn stage_connection_close(
        &mut self,
        rep: u32,
        reason: wit::ConnectionCloseReason,
    ) -> Result<Resource<wit::ConnectionCloseRequest>, wit::ConnectionControlError> {
        use wit::ConnectionControlError as Error;
        let (reference, player) = self
            .live_player_for(rep, PluginCapability::ConnectionControl)
            .map_err(Error::Access)?;
        if reason.message.len()
            > control::MAX_REASON.min(self.api_limits.max_connection_reason_bytes as usize)
            || reason
                .message
                .chars()
                .any(|ch| ch.is_control() && ch != '\n')
        {
            return Err(Error::Access(wit::AccessError::InvalidArgument));
        }
        let world = unsafe { self.world() };
        let connection = ConnectionIdentity::capture(world, player).ok_or(Error::NotConnected)?;
        if world
            .get::<crate::entity::components::PlayerSession>(player)
            .is_none_or(|session| session.outbound_tx.is_closed())
        {
            return Err(Error::NotConnected);
        }
        if world
            .get_resource::<SessionEntityMap>()
            .and_then(|map| map.get(connection.session_id()))
            != Some(player)
        {
            return Err(Error::Access(wit::AccessError::StalePlayer));
        }
        if self.active_admission.is_some() || player_events::initial_state_pending(world, player) {
            return Err(Error::AdmissionPending);
        }
        control::capacity(world, player, &self.pending_connection_closes)?;
        if self.resources.connection_closes >= self.api_limits.max_connection_requests_per_tick {
            return Err(Error::Access(wit::AccessError::LimitExceeded));
        }
        let scope = self
            .resources
            .scope
            .ok_or(Error::Access(wit::AccessError::ExpiredScope))?;
        let reservation = Reservation::acquire(
            &self.connection_close_count,
            self.api_limits.max_pending_connection_requests,
        )
        .map_err(Error::Access)?;
        let state = Arc::new(RequestState::new());
        let rep = self
            .resources
            .allocate(
                Entry::ConnectionClose(state.clone()),
                self.api_limits.max_owned_resources,
            )
            .map_err(Error::Access)?;
        self.resources.connection_closes += 1;
        self.pending_connection_closes.push(Request::new(
            state,
            Source {
                reference,
                world: scope.world,
                player,
                connection,
            },
            reason,
            Arc::downgrade(&self.command_owner),
            reservation,
        ));
        Ok(Resource::new_own(rep))
    }
}
impl super::super::wit_bindings::unastar::plugin::types::HostConnectionCloseRequest
    for HostContext
{
    fn status(
        &mut self,
        request: Resource<wit::ConnectionCloseRequest>,
    ) -> Result<wit::ConnectionCloseStatus, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ConnectionControl)?;
        match self.resources.entries.get(&request.rep()) {
            Some(Entry::ConnectionClose(state)) => Ok(state.status()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn drop(&mut self, request: Resource<wit::ConnectionCloseRequest>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&request.rep());
        Ok(())
    }
}

/// The player host implementation delegates its new import to the same scoped validation path.
pub(super) fn stage(
    context: &mut HostContext,
    rep: u32,
    reason: wit::ConnectionCloseReason,
) -> Result<Resource<wit::ConnectionCloseRequest>, wit::ConnectionControlError> {
    context.stage_connection_close(rep, reason)
}
