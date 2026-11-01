//! Scoped requests over native canonical source ownership; game policy remains in the component.

use super::*;
use crate::plugin::{archive_store, manifest::PluginCapability};
use crate::server::game::player_state::source_actors as native;

#[derive(Default)]
pub(in crate::plugin::manager) struct Pending(Vec<native::Prepared>);

/// A remove disposition leaves ordinary native departure persistence authoritative.
pub(in crate::plugin::manager) fn cancel_live_departure(context: &mut HostContext, player: Entity) {
    context.source_actors.0.retain(|prepared| {
        if prepared.operation.live_for(player) {
            prepared.operation.cancel();
            false
        } else {
            true
        }
    });
}

/// Successful explicit requests commit independently of a veto of the callback's default action.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    for prepared in std::mem::take(&mut context.source_actors.0) {
        if succeeded {
            native::submit(world, prepared);
        } else {
            prepared.operation.cancel();
        }
    }
}

/// Native failures expose no provider paths, encoded state or historical actor identifiers.
pub(super) fn error(error: native::Error) -> wit::StateError {
    match error {
        native::Error::NotInstalled => wit::StateError::NotInstalled,
        native::Error::Conflict | native::Error::NotOwned => wit::StateError::Conflict,
        native::Error::WrongWorld => wit::StateError::Access(wit::AccessError::WrongWorld),
        native::Error::SourceGone => wit::StateError::SourceGone,
        native::Error::Busy => wit::StateError::Busy,
        native::Error::InvalidState => wit::StateError::InvalidImage,
        native::Error::LimitExceeded => wit::StateError::Access(wit::AccessError::LimitExceeded),
        native::Error::RecoveryRequired => wit::StateError::RecoveryRequired,
    }
}

/// Current canonical ownership may be observed without claiming a live player or connection.
fn owner(reference: &native::Reference) -> wit::PlayerIdentity {
    wit::PlayerIdentity {
        uuid: reference.uuid.to_string(),
        authenticated_xuid: reference.xuid.as_deref().and_then(|xuid| xuid.parse().ok()),
        name: reference.name.clone(),
        connection_id: None,
    }
}

impl HostContext {
    /// Stage native work with a byte lease shared by every future worker and actual body.
    pub(super) fn queue_source(
        &mut self,
        intent: native::Intent,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        let lease = self
            .resources
            .value_budget
            .own((), 4096)
            .map_err(wit::StateError::Access)?;
        let budget = self.resources.value_budget.clone();
        let reserve = Arc::new(
            move |bytes| -> Result<Arc<dyn Send + Sync>, native::Error> {
                budget
                    .own((), bytes)
                    .map(|lease| lease as Arc<dyn Send + Sync>)
                    .map_err(|_| native::Error::LimitExceeded)
            },
        );
        let prepared =
            native::prepare(unsafe { self.world() }, intent, lease, reserve).map_err(error)?;
        let resource = self
            .resources
            .allocate(
                Entry::SourceActorOperation(prepared.operation.clone()),
                self.api_limits.max_owned_resources,
            )
            .map_err(wit::StateError::Access)?;
        self.source_actors.0.push(prepared);
        Ok(Resource::new_own(resource))
    }

    /// Capture a real source without withholding its physics, damage, food or effects.
    pub(super) fn checkpoint_live_source(
        &mut self,
        player: u32,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        let (lineage, actor) = self.live_player(player).map_err(wit::StateError::Access)?;
        let world = unsafe { self.world() };
        let retained = world
            .get::<crate::server::game::player_events::DetachedPlayer>(actor)
            .is_some()
            && world
                .get::<crate::entity::components::PlayerSession>(actor)
                .is_none();
        if !retained && self.active_departure != Some(actor) {
            return Err(wit::StateError::SourceGone);
        }
        if crate::server::game::inventory_effects::is_quarantined(world, actor)
            || native::pending(world, actor)
            || world
                .get::<crate::entity::player_life::Dead>(actor)
                .is_some()
            || crate::server::game::player_events::initial_state_pending(world, actor)
        {
            return Err(wit::StateError::Busy);
        }
        let nonce = if let Some(managed) = world.get::<native::ManagedSource>(actor) {
            if managed.record.module != self.plugin_id.0
                || managed.record.phase != crate::storage::source::SourcePhase::Active
            {
                return Err(wit::StateError::Conflict);
            }
            managed.record.nonce.clone()
        } else if let Some(live) = world.get::<native::LiveSource>(actor) {
            if live.0.reference.module != self.plugin_id.0 {
                return Err(wit::StateError::Conflict);
            }
            live.0.reference.nonce.clone()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let image_resource = self
            .capture_state_image(player)
            .map_err(wit::StateError::Access)?;
        let image = self
            .state_image(image_resource.rep())
            .map_err(wit::StateError::Access)?;
        self.resources.entries.remove(&image_resource.rep());
        let reference = native::Reference {
            module: self.plugin_id.0.clone(),
            uuid: uuid::Uuid::parse_str(&image.expected.data.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            xuid: image.expected.xuid.clone(),
            name: image.owner.name.clone(),
            nonce,
            world: uuid::Uuid::parse_str(&image.world.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            dimension: image.world.dimension,
        };
        let intent = native::Intent::Live {
            player: actor,
            lineage,
            snapshot: image.expected.clone(),
            reference,
        };
        // The charged capture must outlive a dropped guest operation and its worker.
        let budget = self.resources.value_budget.clone();
        let reserve = Arc::new(
            move |bytes| -> Result<Arc<dyn Send + Sync>, native::Error> {
                budget
                    .own((), bytes)
                    .map(|lease| lease as Arc<dyn Send + Sync>)
                    .map_err(|_| native::Error::LimitExceeded)
            },
        );
        let prepared =
            native::prepare(unsafe { self.world() }, intent, image, reserve).map_err(error)?;
        let resource = self
            .resources
            .allocate(
                Entry::SourceActorOperation(prepared.operation.clone()),
                self.api_limits.max_owned_resources,
            )
            .map_err(wit::StateError::Access)?;
        self.source_actors.0.push(prepared);
        Ok(Resource::new_own(resource))
    }

    /// A resource is scoped to its creating component and cannot authorize another namespace.
    pub(super) fn source_operation(
        &mut self,
        rep: u32,
    ) -> Result<Arc<native::Operation>, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::RestorePlayerState)?;
        match self.resources.entries.get(&rep) {
            Some(Entry::SourceActorOperation(operation))
                if operation.reference.module == self.plugin_id.0 =>
            {
                Ok(operation.clone())
            }
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }

    /// A staged archive by itself is never evidence that a source hold was physically accepted.
    pub(super) fn inspect_source(
        &mut self,
        archive: u32,
        tx: u32,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.validate_transaction(tx)
            .map_err(wit::StateError::Access)?;
        let archive = self.archived(archive).map_err(wit::StateError::Access)?;
        let record = &archive.loaded.record;
        if record.death.is_some() {
            return Err(wit::StateError::InvalidImage);
        }
        let reference = native::Reference {
            module: self.plugin_id.0.clone(),
            uuid: uuid::Uuid::parse_str(&record.owner.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            xuid: record.owner.authenticated_xuid.map(|xuid| xuid.to_string()),
            name: record.owner.name.clone(),
            nonce: record
                .source_fence
                .clone()
                .ok_or(wit::StateError::InvalidImage)?,
            world: uuid::Uuid::parse_str(&record.world.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            dimension: record.world.dimension,
        };
        self.queue_source(native::Intent::Inspect(reference))
    }

    /// Only a real existing held source can request the durable ownership checkpoint.
    pub(super) fn checkpoint_source(
        &mut self,
        source: u32,
        tx: u32,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.validate_transaction(tx)
            .map_err(wit::StateError::Access)?;
        let source = self.state_fence(source).map_err(wit::StateError::Access)?;
        let world = unsafe { self.world() };
        let hold = source.hold(world).ok_or(wit::StateError::SourceGone)?;
        let reference = native::Reference {
            module: self.plugin_id.0.clone(),
            uuid: source.reference.uuid,
            xuid: source.image.expected.xuid.clone(),
            name: source.reference.identity.name.clone(),
            nonce: source.id.to_string(),
            world: uuid::Uuid::parse_str(&source.image.world.uuid)
                .map_err(|_| wit::StateError::InvalidImage)?,
            dimension: source.world.dimension,
        };
        self.queue_source(native::Intent::Seal {
            player: hold.player,
            hold,
            snapshot: source.image.expected.clone(),
            reference,
        })
    }

    /// Fresh transaction scope and the observed generation are both revalidated by the native worker.
    fn continue_source(
        &mut self,
        rep: u32,
        tx: u32,
        action: native::Action,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.validate_transaction(tx)
            .map_err(wit::StateError::Access)?;
        let observed = self
            .source_operation(rep)
            .map_err(wit::StateError::Access)?;
        self.queue_source(native::Intent::Continue { observed, action })
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostSourceActorOperation for HostContext {
    fn source_fence(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Resource<wit::SourceFence>, wit::StateError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::StateError::Access)?;
        let operation = self
            .source_operation(operation.rep())
            .map_err(wit::StateError::Access)?;
        self.adopt_source_fence(&operation)
    }
    fn status(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
    ) -> Result<wit::SourceActorStatus, wit::AccessError> {
        let operation = self.source_operation(operation.rep())?;
        Ok(match operation.status() {
            native::Status::Staged => wit::SourceActorStatus::Staged,
            native::Status::Queued => wit::SourceActorStatus::Queued,
            native::Status::Committed {
                phase,
                generation,
                live_applied,
                physical,
            } => wit::SourceActorStatus::Observed(wit::SourceActorObservation {
                phase: match phase {
                    native::Phase::Frozen => wit::SourceActorPhase::Frozen,
                    native::Phase::Active => wit::SourceActorPhase::Active,
                    native::Phase::Retired => wit::SourceActorPhase::Retired,
                    native::Phase::Transferred => wit::SourceActorPhase::Transferred,
                    native::Phase::Terminal => wit::SourceActorPhase::Terminal,
                },
                generation,
                live_applied,
                physical: match physical {
                    native::Physical::Unavailable => wit::SourceActorPhysical::Unavailable,
                    native::Physical::Absent => wit::SourceActorPhysical::Absent,
                    native::Physical::Held => wit::SourceActorPhysical::Held,
                    native::Physical::Present => wit::SourceActorPhysical::Present,
                },
            }),
            native::Status::Rejected(native::Error::NotOwned) => wit::SourceActorStatus::Unowned,
            native::Status::Rejected(reason) => wit::SourceActorStatus::Rejected(error(reason)),
            native::Status::Uncertain => wit::SourceActorStatus::Uncertain,
            native::Status::Cancelled => wit::SourceActorStatus::Cancelled,
        })
    }
    fn state_image(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
    ) -> Result<Resource<wit::PlayerStateImage>, wit::AccessError> {
        let operation = self.source_operation(operation.rep())?;
        self.state_from_source(&operation)
    }
    fn archive(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
        key: String,
    ) -> Result<wit::ArchiveReference, wit::ArchiveError> {
        let operation = self
            .source_operation(operation.rep())
            .map_err(wit::ArchiveError::Access)?;
        let observed = operation.observation();
        let provisional = operation.provisional();
        let bytes = match (&observed, &provisional) {
            (Some(observed), _) => observed
                .retained_bytes()
                .map_err(|_| wit::ArchiveError::Access(wit::AccessError::LimitExceeded))?,
            (_, Some(snapshot)) => native::snapshot_bytes(snapshot)
                .map_err(|_| wit::ArchiveError::Access(wit::AccessError::LimitExceeded))?,
            _ => return Err(wit::ArchiveError::Access(wit::AccessError::Unavailable)),
        };
        let _lease = self
            .resources
            .value_budget
            .reserve(
                bytes
                    .checked_mul(2)
                    .ok_or(wit::ArchiveError::Access(wit::AccessError::LimitExceeded))?,
            )
            .map_err(wit::ArchiveError::Access)?;
        let raw = match &observed {
            Some(observed) => observed
                .snapshot()
                .map_err(|_| wit::ArchiveError::InvalidSnapshot)?,
            None => (*provisional.ok_or(wit::ArchiveError::InvalidSnapshot)?).clone(),
        };
        let record = archive_store::Record::new(
            &self.plugin_id.0,
            archive_store::Owner {
                uuid: operation.reference.uuid.to_string(),
                authenticated_xuid: owner(&operation.reference).authenticated_xuid,
                name: operation.reference.name.clone(),
                connection_id: None,
            },
            archive_store::SourceWorld {
                uuid: operation.reference.world.to_string(),
                name: operation.world_name.clone(),
                dimension: operation.reference.dimension,
            },
            operation.tick,
            raw,
        )
        .map_err(|_| wit::ArchiveError::InvalidSnapshot)?
        .with_source_fence(
            uuid::Uuid::parse_str(&operation.reference.nonce)
                .map_err(|_| wit::ArchiveError::InvalidSnapshot)?,
        );
        let record = match observed {
            Some(observed) => record
                .with_source_actor(observed.source.clone())
                .map_err(|_| wit::ArchiveError::InvalidSnapshot)?,
            None => record,
        };
        let reference = self.archive_call(true, |namespace| namespace.capture(&key, &record))?;
        Ok(wit::ArchiveReference {
            key: reference.key,
            revision: reference.revision,
            fingerprint: reference.fingerprint,
            owner: super::archive::identity(&reference.owner),
        })
    }
    fn player(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Option<Resource<wit::PlayerHandle>>, wit::StateError> {
        self.validate_transaction(tx.rep())
            .map_err(wit::StateError::Access)?;
        let operation = self
            .source_operation(operation.rep())
            .map_err(wit::StateError::Access)?;
        let world = self.world_mut();
        let Some(actor) = operation.player(world) else {
            return Ok(None);
        };
        let reference = ensure_identity(world, actor).map_err(wit::StateError::Access)?;
        self.resources
            .allocate(
                Entry::PlayerHandle(reference),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
            .map(Some)
            .map_err(wit::StateError::Access)
    }
    fn recreate(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.continue_source(operation.rep(), tx.rep(), native::Action::Recreate)
    }
    fn activate(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.continue_source(operation.rep(), tx.rep(), native::Action::Activate)
    }
    fn retire(
        &mut self,
        operation: Resource<wit::SourceActorOperation>,
        tx: Resource<wit::WorldTx>,
    ) -> Result<Resource<wit::SourceActorOperation>, wit::StateError> {
        self.continue_source(operation.rep(), tx.rep(), native::Action::Retire)
    }
    fn drop(&mut self, operation: Resource<wit::SourceActorOperation>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&operation.rep());
        Ok(())
    }
}
