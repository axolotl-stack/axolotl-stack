//! Scoped live ownership requests with owned acknowledgements and explicit publication.
use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::player_state::source_actors::{self as sources, rebind as native};
use parking_lot::Mutex;
use std::collections::VecDeque;

#[derive(Clone)]
pub(super) enum Resource {
    Live(Arc<native::Operation>),
    Readback(Arc<native::Readback>),
}
impl Resource {
    fn status(&self) -> native::Status {
        match self {
            Self::Live(operation) => operation.status(),
            Self::Readback(readback) => readback.status(),
        }
    }
}
#[derive(Default)]
pub(in crate::plugin::manager) struct Results(Mutex<VecDeque<wit::SourceRebindResult>>);
#[derive(Default)]
pub(in crate::plugin::manager) struct Requests {
    staged: Vec<Arc<native::Operation>>,
    publish: Vec<Arc<native::Operation>>,
}
fn status(status: native::Status) -> wit::SourceRebindStatus {
    let commit =
        |receipt: crate::storage::InventoryEffectRecord, live_applied| wit::SourceRebindCommit {
            receipt: wit::InventoryReceipt {
                revision: receipt.revision,
                request_hash: receipt.request_hash.to_vec(),
            },
            live_applied,
        };
    match status {
        native::Status::Staged => wit::SourceRebindStatus::Staged,
        native::Status::Queued => wit::SourceRebindStatus::Queued,
        native::Status::Acknowledged(receipt) => {
            wit::SourceRebindStatus::Acknowledged(commit(receipt, false))
        }
        native::Status::Rebound(receipt) => wit::SourceRebindStatus::Rebound(commit(receipt, true)),
        native::Status::Terminal(receipt, applied) => {
            wit::SourceRebindStatus::Terminal(commit(receipt, applied))
        }
        native::Status::Retired(receipt, applied) => {
            wit::SourceRebindStatus::Retired(commit(receipt, applied))
        }
        native::Status::Failed(receipt, reason) => {
            wit::SourceRebindStatus::Failed(wit::SourceRebindFailure {
                receipt: commit(receipt, false).receipt,
                reason: super::source_actors::error(reason),
            })
        }
        native::Status::Unowned => wit::SourceRebindStatus::Unowned,
        native::Status::Rejected(error) => {
            wit::SourceRebindStatus::Rejected(super::source_actors::error(error))
        }
        native::Status::Uncertain => wit::SourceRebindStatus::Uncertain,
        native::Status::Cancelled => wit::SourceRebindStatus::Cancelled,
    }
}
impl HostContext {
    fn source_rebind(&mut self, rep: u32) -> Result<Resource, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::RestorePlayerState)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::SourceRebind(resource)) => Ok(resource.clone()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn own_rebind(
        &mut self,
        resource: Resource,
    ) -> Result<wasmtime::component::Resource<wit::SourceRebindOperation>, wit::StateError> {
        self.resources
            .allocate(
                Entry::SourceRebind(resource),
                self.api_limits.max_owned_resources,
            )
            .map(wasmtime::component::Resource::new_own)
            .map_err(wit::StateError::Access)
    }
    /// Only the exact unresolved native admission hold can nominate its actual replacement.
    pub(super) fn stage_source_rebind(
        &mut self,
        hold: u32,
        tx: u32,
        source: u32,
        correlation: u64,
    ) -> Result<wasmtime::component::Resource<wit::SourceRebindOperation>, wit::StateError> {
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        let owned = match self.resources.entries.get(&hold) {
            Some(Entry::AdmissionHold(owned))
                if owned.owner == self.plugin_id.0 && !owned.staged =>
            {
                owned.clone()
            }
            _ => return Err(wit::StateError::Access(wit::AccessError::StalePlayer)),
        };
        self.initialization_owner(tx, owned.player, hold)
            .map_err(wit::StateError::Access)?;
        let source = self
            .source_operation(source)
            .map_err(wit::StateError::Access)?;
        if source.world != unsafe { self.world() }.id() {
            return Err(wit::StateError::Access(wit::AccessError::WrongWorld));
        }
        let target =
            ensure_identity(self.world_mut(), owned.player).map_err(wit::StateError::Access)?;
        let borrowed = self
            .borrow_player(owned.player)
            .map_err(wit::StateError::Access)?;
        let captured = self
            .capture_state_image(borrowed.rep())
            .map_err(wit::StateError::Access)?;
        let image = self
            .state_image(captured.rep())
            .map_err(wit::StateError::Access)?;
        self.resources.entries.remove(&captured.rep());
        self.resources.entries.remove(&borrowed.rep());
        let valid = owned.clone();
        let ready = owned.clone();
        let allow = owned.clone();
        let reject = owned.clone();
        let authority = native::Authority {
            reject: Arc::new(move |_| {
                reject.book.reject_for_hold(
                    reject.id,
                    &reject.owner,
                    "Unable to complete connection setup.",
                )
            }),
            valid: Arc::new(move |world| {
                matches!(
                    valid.book.status_for_hold(world, valid.id, &valid.owner),
                    super::super::sessions::holds::Status::Ready
                        | super::super::sessions::holds::Status::Waiting
                )
            }),
            ready: Arc::new(move |world| {
                matches!(
                    ready.book.status_for_hold(world, ready.id, &ready.owner),
                    super::super::sessions::holds::Status::Ready
                )
            }),
            allow: Arc::new(move |world| {
                allow
                    .book
                    .resolve(
                        world,
                        allow.id,
                        &allow.owner,
                        super::super::sessions::holds::Decision::Allow,
                    )
                    .is_ok()
            }),
        };
        let budget = self.resources.value_budget.clone();
        let reserve = Arc::new(move |bytes| {
            budget
                .own((), bytes)
                .map(|lease| lease as Arc<dyn Send + Sync>)
                .map_err(|_| sources::Error::LimitExceeded)
        });
        let source_identity = wit::PlayerIdentity {
            uuid: source.reference.uuid.to_string(),
            authenticated_xuid: source
                .reference
                .xuid
                .as_deref()
                .and_then(|value| value.parse().ok()),
            name: source.reference.name.clone(),
            connection_id: None,
        };
        let target_identity = target.identity.clone();
        let results = self.rebind_results.clone();
        // The request identifier is filled after the native constructor allocates it.
        let notice_id = Arc::new(Mutex::new(String::new()));
        let id = notice_id.clone();
        let notice = Arc::new(move |outcome: native::Status| {
            if matches!(outcome, native::Status::Staged | native::Status::Queued) {
                return;
            }
            results.0.lock().push_back(wit::SourceRebindResult {
                request_id: id.lock().clone(),
                correlation,
                source: source_identity.clone(),
                target: target_identity.clone(),
                status: status(outcome),
            });
        });
        let operation = native::prepare(
            unsafe { self.world() },
            source,
            owned.player,
            target,
            image.expected.clone(),
            authority,
            correlation,
            reserve,
            image,
            notice,
        )
        .map_err(super::source_actors::error)?;
        *notice_id.lock() = operation.id.to_string();
        let resource = self.own_rebind(Resource::Live(operation.clone()))?;
        self.source_rebinds.staged.push(operation);
        Ok(resource)
    }
    /// Reload checks require a genuine module-owned canonical source witness and generated ID.
    pub(super) fn inspect_source_rebind(
        &mut self,
        archive: u32,
        tx: u32,
        id: String,
    ) -> Result<wasmtime::component::Resource<wit::SourceRebindOperation>, wit::StateError> {
        self.validate_transaction(tx)
            .map_err(wit::StateError::Access)?;
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        let archive = self.archived(archive).map_err(wit::StateError::Access)?;
        let record = &archive.loaded.record;
        let stamp = record
            .source_actor
            .as_ref()
            .ok_or(wit::StateError::InvalidImage)?;
        if record.death.is_some()
            || stamp.module != self.plugin_id.0
            || stamp.phase != crate::storage::source::SourcePhase::Active
        {
            return Err(wit::StateError::InvalidImage);
        }
        let id = uuid::Uuid::parse_str(&id).map_err(|_| wit::StateError::InvalidIdentifier)?;
        let reference = sources::Reference {
            module: self.plugin_id.0.clone(),
            uuid: uuid::Uuid::parse_str(&record.owner.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            xuid: record
                .owner
                .authenticated_xuid
                .map(|value| value.to_string()),
            name: record.owner.name.clone(),
            nonce: stamp.nonce.clone(),
            world: uuid::Uuid::parse_str(&record.world.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            dimension: record.world.dimension,
        };
        let budget = self.resources.value_budget.clone();
        let reserve = Arc::new(move |bytes| {
            budget
                .own((), bytes)
                .map(|lease| lease as Arc<dyn Send + Sync>)
                .map_err(|_| sources::Error::LimitExceeded)
        });
        let readback = native::inspect(self.world_mut(), reference, stamp.generation, id, reserve)
            .map_err(super::source_actors::error)?;
        self.own_rebind(match readback {
            native::Lookup::Live(operation) => Resource::Live(operation),
            native::Lookup::Readback(readback) => Resource::Readback(readback),
        })
    }
}
/// Explicit publication is staged like every other guest mutation, independently of default vetoes.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    for operation in std::mem::take(&mut context.source_rebinds.staged) {
        if succeeded {
            native::submit(world, operation);
        } else {
            operation.cancel();
        }
    }
    for operation in std::mem::take(&mut context.source_rebinds.publish) {
        if succeeded {
            let _ = native::publish(world, &operation);
        }
    }
}
impl super::super::wit_bindings::unastar::plugin::types::HostSourceRebindOperation for HostContext {
    fn request_id(
        &mut self,
        resource: wasmtime::component::Resource<wit::SourceRebindOperation>,
    ) -> Result<String, wit::AccessError> {
        Ok(match self.source_rebind(resource.rep())? {
            Resource::Live(operation) => operation.id.to_string(),
            Resource::Readback(readback) => readback.id.to_string(),
        })
    }
    fn status(
        &mut self,
        resource: wasmtime::component::Resource<wit::SourceRebindOperation>,
    ) -> Result<wit::SourceRebindStatus, wit::AccessError> {
        Ok(status(self.source_rebind(resource.rep())?.status()))
    }
    fn publish(
        &mut self,
        resource: wasmtime::component::Resource<wit::SourceRebindOperation>,
        tx: wasmtime::component::Resource<wit::WorldTx>,
    ) -> Result<wit::AdmissionAcknowledgement, wit::StateError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::StateError::Access)?;
        let Resource::Live(operation) = self
            .source_rebind(resource.rep())
            .map_err(wit::StateError::Access)?
        else {
            return Err(wit::StateError::SourceGone);
        };
        if operation.source.world != unsafe { self.world() }.id() {
            return Err(wit::StateError::Access(wit::AccessError::WrongWorld));
        }
        if !matches!(operation.status(), native::Status::Acknowledged(_)) {
            return Err(wit::StateError::Busy);
        }
        for entry in self.resources.entries.values_mut() {
            if let Entry::AdmissionHold(hold) = entry
                && hold.player == operation.target
                && hold.owner == self.plugin_id.0
            {
                hold.staged = true;
            }
        }
        self.source_rebinds.publish.push(operation);
        Ok(wit::AdmissionAcknowledgement::Staged)
    }
    fn retire(
        &mut self,
        resource: wasmtime::component::Resource<wit::SourceRebindOperation>,
        tx: wasmtime::component::Resource<wit::WorldTx>,
    ) -> Result<wasmtime::component::Resource<wit::SourceActorOperation>, wit::StateError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::StateError::Access)?;
        let Resource::Live(operation) = self
            .source_rebind(resource.rep())
            .map_err(wit::StateError::Access)?
        else {
            return Err(wit::StateError::SourceGone);
        };
        if !matches!(
            operation.status(),
            native::Status::Acknowledged(_) | native::Status::Failed(_, _)
        ) {
            return Err(wit::StateError::Busy);
        }
        self.queue_source(sources::Intent::RetireRebind { operation })
    }
    fn player(
        &mut self,
        resource: wasmtime::component::Resource<wit::SourceRebindOperation>,
        tx: wasmtime::component::Resource<wit::WorldTx>,
    ) -> Result<Option<wasmtime::component::Resource<wit::PlayerHandle>>, wit::StateError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::StateError::Access)?;
        let Resource::Live(operation) = self
            .source_rebind(resource.rep())
            .map_err(wit::StateError::Access)?
        else {
            return Ok(None);
        };
        let Some(player) = operation.player(unsafe { self.world() }) else {
            return Ok(None);
        };
        let reference =
            ensure_identity(self.world_mut(), player).map_err(wit::StateError::Access)?;
        self.resources
            .allocate(
                Entry::PlayerHandle(reference),
                self.api_limits.max_owned_resources,
            )
            .map(wasmtime::component::Resource::new_own)
            .map(Some)
            .map_err(wit::StateError::Access)
    }
    fn drop(
        &mut self,
        resource: wasmtime::component::Resource<wit::SourceRebindOperation>,
    ) -> wasmtime::Result<()> {
        self.resources.entries.remove(&resource.rep());
        Ok(())
    }
}
impl super::super::PluginManager {
    /// Deliver owned results outside every native world mutation and component-manager reentry.
    pub(crate) fn drain_source_rebind_results(
        &mut self,
        router: &mut crate::server::worlds::DimensionRouter,
    ) {
        if self.readiness.check().is_err() {
            return;
        }
        let Some(server) = router.world_mut(router.primary_dimension()) else {
            return;
        };
        let world = server.ecs.world_mut();
        let mut owners: Vec<_> = self.plugins.keys().cloned().collect();
        owners.sort_by(|a, b| a.0.cmp(&b.0));
        for owner in owners {
            let plugin = self.plugins.get_mut(&owner).expect("listed component");
            let notices: Vec<_> = plugin
                .store
                .data()
                .rebind_results
                .0
                .lock()
                .drain(..)
                .collect();
            for notice in notices {
                let result = super::super::lifecycle::invoke(plugin, world, |store, bindings| {
                    bindings.call_on_source_rebind_result(store, &notice)
                });
                if let Err(error) = result {
                    self.callback_failed(world, &owner, &error.to_string());
                    break;
                }
            }
        }
    }
}
