//! Admission tokens use the same component resource budget and callback scope validation.

use super::super::sessions::{OwnedHold, StagedResolution, holds};
use super::*;

/// Keep admission errors semantic and independent of native host types.
fn error(error: holds::Error) -> wit::AccessError {
    match error {
        holds::Error::Stale => wit::AccessError::StalePlayer,
        holds::Error::WrongOwner => wit::AccessError::PermissionDenied,
        holds::Error::InvalidDecision => wit::AccessError::InvalidArgument,
        holds::Error::Limit => wit::AccessError::LimitExceeded,
        holds::Error::Unavailable => wit::AccessError::Unavailable,
    }
}
impl HostContext {
    /// Scoped inventory/restore bindings use this authority while ordinary gameplay remains closed.
    pub(in crate::plugin::manager) fn initialization_owner(
        &mut self,
        tx: u32,
        player: Entity,
        hold: u32,
    ) -> Result<(), wit::AccessError> {
        self.validate_transaction(tx)?;
        let hold = match self.resources.entries.get(&hold) {
            Some(Entry::AdmissionHold(hold)) if hold.owner == self.plugin_id.0 && !hold.staged => {
                hold.clone()
            }
            _ => return Err(wit::AccessError::StalePlayer),
        };
        let active = self
            .active_admission
            .as_ref()
            .filter(|active| active.player == player && active.book.same(&hold.book))
            .map(|active| active.id);
        hold.book
            .validate_initialization(
                unsafe { self.world() },
                hold.id,
                &hold.owner,
                player,
                active,
            )
            .map_err(error)
    }

    #[cfg(test)]
    pub(in crate::plugin::manager) fn test_admission_hold(&self) -> Option<u32> {
        self.resources
            .entries
            .iter()
            .find_map(|(rep, entry)| matches!(entry, Entry::AdmissionHold(_)).then_some(*rep))
    }

    /// Only the current player's admission proposal can create provisional holds.
    pub(super) fn admission_hold(
        &mut self,
        player: u32,
    ) -> Result<Resource<wit::AdmissionHold>, wit::AccessError> {
        let (_, player) = self.live_player(player)?;
        let active = self
            .active_admission
            .clone()
            .ok_or(wit::AccessError::Unavailable)?;
        if active.player != player
            || active.book.player(unsafe { self.world() }, active.id) != Some(player)
        {
            return Err(wit::AccessError::StalePlayer);
        }
        if self.resources.entries.len() >= self.api_limits.max_owned_resources as usize {
            return Err(wit::AccessError::LimitExceeded);
        }
        let id = active
            .book
            .hold(active.id, &self.plugin_id.0)
            .map_err(error)?;
        let hold = OwnedHold {
            book: active.book,
            id,
            player,
            owner: self.plugin_id.0.clone(),
            staged: false,
        };
        self.resources
            .allocate(
                Entry::AdmissionHold(hold.clone()),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
            .inspect_err(|_| {
                let _ = hold.book.abandoned(hold.id, &hold.owner);
            })
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostAdmissionHold for HostContext {
    fn submit_inventory(
        &mut self,
        hold: Resource<wit::AdmissionHold>,
        tx: Resource<wit::WorldTx>,
        request_id: String,
        image: Resource<wit::InventoryImage>,
    ) -> Result<Resource<wit::InventoryOperation>, wit::InventoryError> {
        self.submit_initial_inventory(hold.rep(), tx.rep(), request_id, image.rep())
    }
    fn resolve(
        &mut self,
        resource: Resource<wit::AdmissionHold>,
        tx: Resource<wit::WorldTx>,
        decision: wit::AdmissionResolution,
    ) -> Result<wit::AdmissionAcknowledgement, wit::AccessError> {
        let hold = match self.resources.entries.get(&resource.rep()) {
            Some(Entry::AdmissionHold(hold)) if hold.owner == self.plugin_id.0 && !hold.staged => {
                hold.clone()
            }
            _ => return Err(wit::AccessError::StalePlayer),
        };
        self.initialization_owner(tx.rep(), hold.player, resource.rep())?;
        let decision = match decision {
            wit::AdmissionResolution::Allow => holds::Decision::Allow,
            wit::AdmissionResolution::Reject(reason) => {
                if reason.is_empty()
                    || reason.len() > 1024
                    || reason.chars().any(|ch| ch.is_control() && ch != '\n')
                {
                    return Err(wit::AccessError::InvalidArgument);
                }
                holds::Decision::Reject(reason)
            }
        };
        // Validate ownership now, and revalidate it again when the callback commits.
        hold.book
            .validate_resolution(unsafe { self.world() }, hold.id, &hold.owner)
            .map_err(error)?;
        if let Some(Entry::AdmissionHold(owned)) = self.resources.entries.get_mut(&resource.rep()) {
            owned.staged = true;
        }
        self.pending_admissions
            .push(StagedResolution { hold, decision });
        Ok(wit::AdmissionAcknowledgement::Staged)
    }
    fn drop(&mut self, resource: Resource<wit::AdmissionHold>) -> wasmtime::Result<()> {
        if let Some(Entry::AdmissionHold(hold)) = self.resources.entries.remove(&resource.rep())
            && !hold.staged
        {
            let _ = hold.book.abandoned(hold.id, &hold.owner);
        }
        Ok(())
    }
}

/// Failed callbacks reject retained holds even when a trapped instance cannot run destructors.
pub(super) fn finish(context: &mut HostContext, world: &mut World, succeeded: bool) {
    let resolutions = std::mem::take(&mut context.pending_admissions);
    if !succeeded {
        for entry in context.resources.entries.values() {
            if let Entry::AdmissionHold(hold) = entry {
                hold.book.owner_failed(&context.plugin_id.0);
            }
        }
        for action in resolutions {
            action.hold.book.owner_failed(&context.plugin_id.0);
        }
        return;
    }
    for action in resolutions {
        let _ =
            action
                .hold
                .book
                .resolve(world, action.hold.id, &action.hold.owner, action.decision);
    }
}
